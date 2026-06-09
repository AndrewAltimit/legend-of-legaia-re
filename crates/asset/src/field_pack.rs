//! Field-pack container - a magic-stamped block carried by a handful of
//! field/town scene PROT entries.
//!
//! ## Format
//!
//! The magic `0x01059B84` appears **raw in exactly four PROT entries**
//! (`0002_gameover_data`, `0003`/`0004`/`0005_town01`); the 97-entry schema
//! *signature* appears in **eight** (the other four — `0020_town0b`,
//! `0021`/`0022`/`0023_town0c` — carry it **without** the magic prefix). The
//! magic is a build-tool stamp, not a runtime parser anchor (a SCUS + overlay
//! scan finds zero references to it). Layout of a carrier:
//!
//! ```text
//! [file start]
//!   ...preamble - the PER-SCENE payload (count + u16 offset table + records)...
//!   [u32 LE = MAGIC = 0x01059B84]   (present in only 4 of the 8 carriers)
//!   [97 × u32 LE - schema table, byte-identical everywhere]
//!   [≈ 91 KB schema-indexed region - a byte-identical GLOBAL CONSTANT block]
//!   [packed TIMs / TMDs - in some files]
//! [file end]
//! ```
//!
//! The 97 schema entries are ascending u32 LE values from `0x60` to `0x16651`,
//! the same in every carrier. **The ≈ 91 KB region the schema indexes is a
//! global constant** — byte-identical (FNV/SHA `c85d6a44d742…`) across town01
//! AND town0c. So the schema slots are a fixed template, **not** filled
//! per-scene; the per-scene field data is the preamble. (Corrected from a raw
//! disc scan — the earlier "124 entries / preamble fills the slots" reading was
//! wrong; see `docs/formats/field-pack.md` and `tests/field_pack_real.rs`.)
//! This parser locates the magic-prefixed schema + the packed asset region
//! after it.
//!
//! ## What this gives us
//!
//! - Reliable detection (`detect`) with no false positives - the magic plus
//!   the strict ascending-u32 schema is a high-bar signature.
//! - Boundary information: where the preamble ends, where the TIMs start,
//!   how many sub-record slots the schema declares, and the implied size of
//!   each slot from `offset[i+1] - offset[i]`.
//! - A per-PROT-entry classifier so downstream tooling can route fieldpack
//!   PROT entries through this parser and everything else through the older
//!   detectors in [`crate::categorize`].
//!
//! ## What this doesn't do
//!
//! - Decode the **preamble** (the per-scene payload before the magic). It is a
//!   count + `u16` offset table + records — the same shape the magic-less
//!   town0b / town0c field files open with — i.e. a scene event/actor
//!   structure, not yet fully decoded here. (There is no "map preamble bytes to
//!   schema slots" step: the schema-indexed region is a global constant, so the
//!   slots are a fixed template, not per-scene-filled. The earlier
//!   "runtime-reconstructed projection" framing was based on the false premise
//!   that the slots hold per-scene data.)
//! - Walk the TIM region. [`crate::tim_scan`] already enumerates TIMs by
//!   magic-scanning the raw bytes, which is sufficient for now.

use serde::Serialize;

/// Magic word that immediately precedes the 97-entry schema table.
pub const MAGIC: u32 = 0x0105_9B84;

/// Structural interpretation of a field-pack schema slot, derived from its
/// byte size.  The size-to-kind mapping is based on the cluster analysis in
/// `docs/formats/field-pack.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SlotKind {
    /// Single-byte flag / type marker (size 1, always slot 0).
    TypeFlag,
    /// Large texture blob, consistent with a TIM page (size `0x2088`).
    /// Five slots: 1, 2, 3, 30, 41.
    TimPage,
    /// NPC slot record - part of a 21-entry tabular array (size `0x218`).
    /// Slots 5–25.
    NpcRecord,
    /// Dialog-trigger or event-region record (size `0x110`).
    EventTrigger,
    /// Collision-box-sized record (size `0x90`).
    CollisionBox,
    /// Compact record (size `0x210`).
    CompactRecord,
    /// Medium record (size `0x410` or `0x1010` - two count buckets).
    MediumRecord,
    /// Any other single-occurrence record with a known size.
    SingleRecord,
    /// Last slot in the schema; size cannot be computed from the schema alone.
    LastSlot,
}

impl SlotKind {
    /// Classify a slot by its byte size (or `None` for the last slot).
    pub fn from_size(size: Option<u32>) -> Self {
        match size {
            None => SlotKind::LastSlot,
            Some(1) => SlotKind::TypeFlag,
            Some(0x2088) => SlotKind::TimPage,
            Some(0x218) => SlotKind::NpcRecord,
            Some(0x110) => SlotKind::EventTrigger,
            Some(0x90) => SlotKind::CollisionBox,
            Some(0x210) => SlotKind::CompactRecord,
            Some(0x410) | Some(0x1010) => SlotKind::MediumRecord,
            Some(_) => SlotKind::SingleRecord,
        }
    }
}

/// Number of u32 entries in the schema table.
pub const RECORD_COUNT: usize = 97;

/// Size of the schema table in bytes.
pub const SCHEMA_SIZE: usize = RECORD_COUNT * 4;

/// First value in the schema (= start of first abstract record).
pub const SCHEMA_FIRST: u32 = 0x60;

/// Last value in the schema (= start of the 97th abstract record).
pub const SCHEMA_LAST: u32 = 0x16651;

/// The 97 schema slot offsets. Byte-identical across every carrier (the 8
/// schema-bearing PROT entries; MD5 `edcfdf1575889d63d2077c396089d7f3`);
/// exposed as a static array so callers can interpret schema slots without
/// parsing a concrete file. Sourced from `0005_town01.BIN` which has the
/// schema table at byte offset 0x4 (preamble-less, template-only layout).
#[rustfmt::skip]
pub const CANONICAL_SCHEMA: [u32; RECORD_COUNT] = [
    0x00060, 0x00061, 0x020E9, 0x04171, 0x061F9, 0x06609, 0x06821, 0x06A39,
    0x06C51, 0x06E69, 0x07081, 0x07299, 0x074B1, 0x076C9, 0x078E1, 0x07AF9,
    0x07D11, 0x07F29, 0x08141, 0x08359, 0x08571, 0x08789, 0x089A1, 0x08BB9,
    0x08DD1, 0x08FE9, 0x09201, 0x09541, 0x09751, 0x097E1, 0x098B1, 0x0B939,
    0x0BA49, 0x0BE59, 0x0C069, 0x0C279, 0x0C589, 0x0C799, 0x0C829, 0x0C8B9,
    0x0C949, 0x0C9D9, 0x0EA61, 0x0FA71, 0x10A81, 0x10E91, 0x112A1, 0x113B1,
    0x114C1, 0x11551, 0x115E1, 0x116F1, 0x11781, 0x11811, 0x11A21, 0x11B51,
    0x11C61, 0x11D61, 0x12371, 0x12401, 0x12511, 0x12621, 0x12A31, 0x12C41,
    0x12CD1, 0x12D61, 0x12E61, 0x13271, 0x13371, 0x13481, 0x13691, 0x13821,
    0x138B1, 0x13BC1, 0x13CD1, 0x13EE1, 0x13F71, 0x14081, 0x14191, 0x142A1,
    0x143B1, 0x14501, 0x14611, 0x14741, 0x14BD1, 0x14CE1, 0x14EF1, 0x14F81,
    0x15091, 0x152A1, 0x15371, 0x15481, 0x15991, 0x15A21, 0x15C31, 0x16441,
    0x16651,
];

/// Convenience accessor: return slot `i` of the [`CANONICAL_SCHEMA`] as
/// `(offset, size)`. Size is `None` for the last slot.
pub fn canonical_slot(i: usize) -> Option<(u32, Option<u32>)> {
    let off = *CANONICAL_SCHEMA.get(i)?;
    let size = CANONICAL_SCHEMA.get(i + 1).map(|next| next - off);
    Some((off, size))
}

/// Iterate `(slot_index, kind, offset, size)` over the canonical
/// 97-slot schema. Useful for downstream tooling that wants to enumerate
/// the static schema without holding any concrete field-pack buffer.
pub fn iter_canonical_slots() -> impl Iterator<Item = (usize, SlotKind, u32, Option<u32>)> {
    (0..RECORD_COUNT).map(|i| {
        let (off, size) = canonical_slot(i).unwrap();
        (i, SlotKind::from_size(size), off, size)
    })
}

/// Parsed location and slot layout of a fieldpack inside a PROT entry buffer.
#[derive(Debug, Clone, Serialize)]
pub struct FieldPack {
    /// File offset of the 4-byte magic word.
    pub magic_offset: usize,
    /// File offset of the first byte of the 97-entry schema table.
    pub table_offset: usize,
    /// File offset immediately after the schema table - first byte of the
    /// packed-TIM region.
    pub assets_start: usize,
    /// Total file size, for convenience when reporting.
    pub file_size: usize,
    /// 97 abstract record slots, each `(offset, size)`. Sizes are derived
    /// from `offset[i+1] - offset[i]`; the last slot's size is unknown and
    /// reported as `None`.
    pub slots: Vec<SchemaSlot>,
}

/// One abstract record slot from the schema.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SchemaSlot {
    /// Offset of this record in the schema's abstract coordinate space.
    /// **Not** a file offset.
    pub offset: u32,
    /// Size of this record (`offset[i+1] - offset[i]`); `None` for the last
    /// slot, whose size depends on per-file preamble layout that we don't
    /// yet decode.
    pub size: Option<u32>,
}

impl FieldPack {
    /// File-offset range of the preamble (before the magic). Its content
    /// shape is unknown - see module docs.
    pub fn preamble_range(&self) -> (usize, usize) {
        (0, self.magic_offset)
    }

    /// File-offset range of the asset region (TIMs + TMDs after the schema).
    pub fn assets_range(&self) -> (usize, usize) {
        (self.assets_start, self.file_size)
    }

    /// Group the schema slots by size and return the buckets in size-descending
    /// order. Each bucket lists the slot indices that share the same size.
    /// The last slot (`size = None`) is excluded.
    ///
    /// The retail field-pack schema is byte-identical across every instance
    /// (see module docs), so the cluster output is a static tabular index of
    /// the slot semantics: slots in the same bucket are *the same kind* of
    /// record.
    pub fn slot_size_groups(&self) -> Vec<(u32, Vec<usize>)> {
        let mut by_size: std::collections::BTreeMap<u32, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(sz) = slot.size {
                by_size.entry(sz).or_default().push(i);
            }
        }
        let mut groups: Vec<(u32, Vec<usize>)> = by_size.into_iter().collect();
        groups.sort_by_key(|(sz, idxs)| (std::cmp::Reverse(idxs.len()), std::cmp::Reverse(*sz)));
        groups
    }

    /// Borrow the bytes of slot `i` from `buf` *if* this field-pack has the
    /// schema-indexed buffer concatenated to the asset region (i.e.
    /// `preamble_size == 0`). For entries where the preamble holds the
    /// schema-indexed buffer, the runtime indirection is more complex and
    /// this helper returns `None`.
    ///
    /// Reads `buf[assets_start + slot[i].offset .. assets_start + slot[i].offset + slot[i].size]`
    /// when in bounds; falls back to `None` for the last slot (size unknown).
    pub fn slot_bytes_in_assets<'a>(&self, buf: &'a [u8], i: usize) -> Option<&'a [u8]> {
        if self.magic_offset != 0 {
            return None;
        }
        let slot = self.slots.get(i)?;
        let size = slot.size? as usize;
        let off = self.assets_start.checked_add(slot.offset as usize)?;
        let end = off.checked_add(size)?;
        if end > buf.len() {
            return None;
        }
        Some(&buf[off..end])
    }

    /// Return the [`SlotKind`] for slot `i`, derived from the slot's byte
    /// size.  Returns `None` if `i` is out of range.
    pub fn slot_kind(&self, i: usize) -> Option<SlotKind> {
        Some(SlotKind::from_size(self.slots.get(i)?.size))
    }

    /// Iterate over all 97 schema slots, yielding `(SlotKind, &[u8])` pairs.
    ///
    /// The byte slice is populated only when `magic_offset == 0` (i.e. the
    /// schema-indexed data sits directly in the asset region, as in entry
    /// `0005_town01`).  For all other entries the preamble holds the
    /// schema-indexed buffer via a runtime-reconstructed indirection that is
    /// not yet traced; those slots yield an empty slice.
    pub fn iter_slots<'a>(
        &'a self,
        buf: &'a [u8],
    ) -> impl Iterator<Item = (SlotKind, &'a [u8])> + 'a {
        self.slots.iter().enumerate().map(move |(i, slot)| {
            let kind = SlotKind::from_size(slot.size);
            let bytes = self.slot_bytes_in_assets(buf, i).unwrap_or(&[]);
            (kind, bytes)
        })
    }
}

/// Look for a fieldpack in `buf`. Returns the first match, scanning forward.
///
/// Detection criteria (all must hold):
/// 1. `MAGIC` (LE) appears at some offset `m`.
/// 2. 388 bytes follow at `m + 4` and lie within the buffer.
/// 3. Those 388 bytes parse as 97 strictly-ascending u32 LE values.
/// 4. `slots[0] == 0x60` and `slots[96] == 0x16651`.
///
/// The combination of magic + strict shape + boundary anchors is specific
/// enough that incidental hits are vanishingly unlikely.
pub fn detect(buf: &[u8]) -> Option<FieldPack> {
    let magic_bytes = MAGIC.to_le_bytes();
    let mut search_from = 0usize;
    while let Some(rel) = find_subslice(&buf[search_from..], &magic_bytes) {
        let magic_offset = search_from + rel;
        if let Some(fp) = parse_at(buf, magic_offset) {
            return Some(fp);
        }
        search_from = magic_offset + 1;
    }
    None
}

fn parse_at(buf: &[u8], magic_offset: usize) -> Option<FieldPack> {
    let table_offset = magic_offset + 4;
    let assets_start = table_offset + SCHEMA_SIZE;
    if assets_start > buf.len() {
        return None;
    }
    let mut slots = Vec::with_capacity(RECORD_COUNT);
    let mut prev: Option<u32> = None;
    for i in 0..RECORD_COUNT {
        let p = table_offset + i * 4;
        let v = u32::from_le_bytes(buf[p..p + 4].try_into().unwrap());
        if let Some(prev_v) = prev
            && v <= prev_v
        {
            return None;
        }
        prev = Some(v);
        slots.push(SchemaSlot {
            offset: v,
            size: None,
        });
    }
    if slots[0].offset != SCHEMA_FIRST || slots[RECORD_COUNT - 1].offset != SCHEMA_LAST {
        return None;
    }
    for i in 0..RECORD_COUNT - 1 {
        let next = slots[i + 1].offset;
        let cur = slots[i].offset;
        slots[i].size = Some(next - cur);
    }
    Some(FieldPack {
        magic_offset,
        table_offset,
        assets_start,
        file_size: buf.len(),
        slots,
    })
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic fieldpack header that satisfies the detector.
    fn synthetic(preamble: usize) -> Vec<u8> {
        let mut buf = vec![0u8; preamble];
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        // Build a strictly-ascending schema with first=0x60 and last=0x16651.
        // Distribute the remaining 95 values evenly between them.
        let span = SCHEMA_LAST - SCHEMA_FIRST;
        let step = span / (RECORD_COUNT as u32 - 1);
        for i in 0..RECORD_COUNT {
            let v = if i == 0 {
                SCHEMA_FIRST
            } else if i == RECORD_COUNT - 1 {
                SCHEMA_LAST
            } else {
                SCHEMA_FIRST + step * i as u32
            };
            buf.extend_from_slice(&v.to_le_bytes());
        }
        // Trailing data - pretend there's some asset bytes after the table.
        buf.extend_from_slice(&[0xAAu8; 64]);
        buf
    }

    #[test]
    fn detects_synthetic_fieldpack() {
        let buf = synthetic(1024);
        let fp = detect(&buf).expect("should detect");
        assert_eq!(fp.magic_offset, 1024);
        assert_eq!(fp.table_offset, 1028);
        assert_eq!(fp.assets_start, 1028 + SCHEMA_SIZE);
        assert_eq!(fp.slots.len(), RECORD_COUNT);
        assert_eq!(fp.slots[0].offset, SCHEMA_FIRST);
        assert_eq!(fp.slots[RECORD_COUNT - 1].offset, SCHEMA_LAST);
        assert!(fp.slots[RECORD_COUNT - 1].size.is_none());
        assert!(fp.slots[0].size.is_some());
    }

    #[test]
    fn rejects_buffer_with_only_magic() {
        let mut buf = vec![0u8; 100];
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        // No table follows.
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_non_ascending_table() {
        let mut buf = vec![0u8; 100];
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        // First entry correct but second goes backward.
        buf.extend_from_slice(&SCHEMA_FIRST.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        for _ in 2..RECORD_COUNT {
            buf.extend_from_slice(&0u32.to_le_bytes());
        }
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_wrong_anchors() {
        // Ascending and 97 entries, but boundary values don't match.
        let mut buf = vec![0u8; 100];
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        for i in 0..RECORD_COUNT {
            buf.extend_from_slice(&((i as u32) * 4).to_le_bytes());
        }
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn slot_sizes_sum_to_known_range() {
        let buf = synthetic(0);
        let fp = detect(&buf).unwrap();
        let total: u32 = fp.slots.iter().filter_map(|s| s.size).sum();
        assert_eq!(total, SCHEMA_LAST - SCHEMA_FIRST);
    }

    #[test]
    fn slot_size_groups_partitions_slots_by_size() {
        let buf = synthetic(0);
        let fp = detect(&buf).unwrap();
        let groups = fp.slot_size_groups();
        // The synthetic schema is built with a constant step, so all but
        // the last slot should share one size, with the residual in a
        // separate bucket. Verify every (size != None) slot ended up in
        // some bucket.
        let total_grouped: usize = groups.iter().map(|(_, v)| v.len()).sum();
        let with_size = fp.slots.iter().filter(|s| s.size.is_some()).count();
        assert_eq!(total_grouped, with_size);
    }

    #[test]
    fn slot_bytes_in_assets_returns_none_when_preamble_present() {
        let buf = synthetic(1024);
        let fp = detect(&buf).unwrap();
        // magic_offset is 1024, so this helper is only valid when 0.
        assert!(fp.slot_bytes_in_assets(&buf, 0).is_none());
    }

    #[test]
    fn slot_kind_from_size_classifies_known_sizes() {
        assert_eq!(SlotKind::from_size(Some(1)), SlotKind::TypeFlag);
        assert_eq!(SlotKind::from_size(Some(0x2088)), SlotKind::TimPage);
        assert_eq!(SlotKind::from_size(Some(0x218)), SlotKind::NpcRecord);
        assert_eq!(SlotKind::from_size(Some(0x110)), SlotKind::EventTrigger);
        assert_eq!(SlotKind::from_size(Some(0x90)), SlotKind::CollisionBox);
        assert_eq!(SlotKind::from_size(Some(0x210)), SlotKind::CompactRecord);
        assert_eq!(SlotKind::from_size(Some(0x410)), SlotKind::MediumRecord);
        assert_eq!(SlotKind::from_size(Some(0x1010)), SlotKind::MediumRecord);
        assert_eq!(SlotKind::from_size(Some(0x810)), SlotKind::SingleRecord);
        assert_eq!(SlotKind::from_size(None), SlotKind::LastSlot);
    }

    #[test]
    fn slot_kind_method_returns_none_out_of_range() {
        let buf = synthetic(0);
        let fp = detect(&buf).unwrap();
        assert!(fp.slot_kind(RECORD_COUNT).is_none());
        assert!(fp.slot_kind(0).is_some());
    }

    #[test]
    fn iter_slots_yields_record_count_items() {
        let buf = synthetic(0);
        let fp = detect(&buf).unwrap();
        let slots: Vec<_> = fp.iter_slots(&buf).collect();
        assert_eq!(slots.len(), RECORD_COUNT);
    }

    #[test]
    fn iter_slots_last_is_last_slot_kind() {
        let buf = synthetic(0);
        let fp = detect(&buf).unwrap();
        let last = fp.iter_slots(&buf).last().unwrap();
        assert_eq!(last.0, SlotKind::LastSlot);
    }

    #[test]
    fn iter_slots_bytes_empty_when_preamble_present() {
        let buf = synthetic(1024);
        let fp = detect(&buf).unwrap();
        // All bytes should be empty since magic_offset != 0.
        for (_kind, bytes) in fp.iter_slots(&buf) {
            assert!(bytes.is_empty());
        }
    }

    #[test]
    fn canonical_schema_anchors_match_global_constants() {
        assert_eq!(CANONICAL_SCHEMA.len(), RECORD_COUNT);
        assert_eq!(CANONICAL_SCHEMA[0], SCHEMA_FIRST);
        assert_eq!(CANONICAL_SCHEMA[RECORD_COUNT - 1], SCHEMA_LAST);
    }

    #[test]
    fn canonical_schema_is_strictly_ascending() {
        for w in CANONICAL_SCHEMA.windows(2) {
            assert!(w[0] < w[1], "schema must be strictly ascending");
        }
    }

    #[test]
    fn canonical_slot_returns_size_for_inner_slots_and_none_for_last() {
        assert_eq!(canonical_slot(0), Some((0x60, Some(1))));
        assert_eq!(canonical_slot(1), Some((0x61, Some(0x2088))));
        assert_eq!(canonical_slot(RECORD_COUNT - 1), Some((SCHEMA_LAST, None)));
        assert_eq!(canonical_slot(RECORD_COUNT), None);
    }

    #[test]
    fn iter_canonical_slots_yields_record_count_with_known_kinds() {
        let mut npc_count = 0usize;
        let mut event_count = 0usize;
        let mut collision_count = 0usize;
        let mut tim_page_count = 0usize;
        for (_, kind, _, _) in iter_canonical_slots() {
            match kind {
                SlotKind::NpcRecord => npc_count += 1,
                SlotKind::EventTrigger => event_count += 1,
                SlotKind::CollisionBox => collision_count += 1,
                SlotKind::TimPage => tim_page_count += 1,
                _ => {}
            }
        }
        assert_eq!(npc_count, 21);
        assert_eq!(event_count, 17);
        assert_eq!(collision_count, 16);
        assert_eq!(tim_page_count, 5);
    }
}
