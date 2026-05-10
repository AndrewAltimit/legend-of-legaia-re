//! Field-pack container - the most common shape under PROT entries that hold
//! field/town/dungeon scene data.
//!
//! ## Format
//!
//! 124 PROT entries (out of 1234) share an identical 388-byte schema block.
//! The schema is preceded by a 4-byte magic and followed by packed PSX TIMs
//! and Legaia TMDs:
//!
//! ```text
//! [file start]
//!   ...preamble (variable size, content shape currently unknown)...
//!   [zero padding to 4-byte alignment]
//!   [u32 LE = MAGIC = 0x01059B84]
//!   [97 × u32 LE - schema table, identical across all 124 instances]
//!   [packed TIMs - back-to-back, each TIM begins with 0x10000000 magic]
//!   [packed TMDs - each preceded by a u32 LE size header]
//! [file end]
//! ```
//!
//! The 97 schema entries are ascending u32 LE values from `0x60` to `0x16651`.
//! They are the *same offsets in every fieldpack* - i.e. the schema describes
//! a static abstract sub-record layout, not file-relative offsets. The
//! preamble bytes that fill those slots vary per-scene; the runtime mapping
//! between preamble bytes and schema slots is not yet understood, so this
//! parser only locates the schema and the packed asset regions after it.
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
//! ## What this doesn't (yet) do
//!
//! - Map preamble bytes to schema slots. The schema offsets cover a 91 KB
//!   range, but in many fieldpack files the preamble is only ~47 KB -
//!   meaning the offsets cannot be plain file-relative byte offsets. They
//!   may index into a runtime-reconstructed buffer (preamble decompressed
//!   into a fixed-shape RAM region), but the reconstruction step has not
//!   been traced yet.
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
}
