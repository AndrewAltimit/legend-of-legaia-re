//! Per-scene field map (`DATA\FIELD\<scene>.MAP`) - the fixed `0x12000`-byte
//! blob at slot 0 of every scene's CDNAME block.
//!
//! ### What it is
//!
//! Every scene block on the disc opens with an entry of **exactly `0x12000`
//! bytes**, and that size is not a coincidence: it is the exact sum of the four
//! regions the runtime addresses off the per-scene field buffer
//! `*(0x1F8003EC)`. The scene loader streams the entry verbatim into that
//! buffer, so a region's file offset and its runtime offset are the same
//! number.
//!
//! ```text
//! +0x00000  0x4000  object / actor descriptor table  (0x20 stride, 512 slots)
//! +0x04000  0x4000  collision + floor grid           (1 byte/tile, 0x80 rows)
//! +0x08000  0x8000  per-tile object-index map        (u16/tile, 0x100 rows)
//! +0x10000  0x2000  per-tile trigger block           (header + 4 sub-tables)
//! ---------------
//!  = 0x12000
//! ```
//!
//! ### Provenance
//!
//! Every offset above is read straight from a loader / consumer's
//! **disassembly**, not from the decompiled C:
//!
//! * `+0x10000` trigger block and the `+0x12000` fallback window -
//!   `FUN_801D5630` (`lui a3,0x1` then `ori v0,v0,0x2000`), which calls the
//!   sub-table walker twice. `see ghidra/scripts/funcs/overlay_cutscene_mapview_801d5630.txt`.
//! * Trigger sub-table header math - `FUN_801D5AE0`:
//!   `sll v1,a0,0x2` / `lh v0,0x2(v1)` / `lh v1,0x4(v1)` (offset + count at
//!   `block + 4*kind + 2` / `+ 4`), stride from `DAT_8007B318 + kind`
//!   (`addiu v0,v0,-0x4ce8` off `lui 0x8008`), tile match on `rec[0]` / `rec[1]`.
//!   `see ghidra/scripts/funcs/overlay_cutscene_mapview_801d5ae0.txt`.
//! * `+0x4000` collision grid with `0x80`-byte rows - `FUN_80019278`:
//!   `sll v0,a2,0x7` then `addiu v0,v0,0x4000`, plus the `lbu v1,0x80(s0)`
//!   next-row neighbour loads. `see ghidra/scripts/funcs/80019278.txt`.
//! * `+0x8000` object-index map with `0x100`-byte rows - same function:
//!   `sll v0,v0,0x7` / `andi v0,v0,0x7f00` / `ori v0,zero,0x8000`.
//! * `0x20`-byte object descriptors indexed by the cell's low 9 bits -
//!   `FUN_8003A55C`: `ori v0,zero,0x8000`, `andi s2,v0,0x1ff`, `sll v0,s2,0x5`,
//!   `lbu v0,0x4000(v0)`. `see ghidra/scripts/funcs/8003a55c.txt`.
//! * `+0x12000` as the region boundary - `FUN_8001F7C0` stages the `.PCH`
//!   sidecar there (`lui v0,0x1` / `ori v0,v0,0x2000`, `li a2,0x800` zero-fill
//!   when the open fails) and sets `_DAT_8007B8D0 = base + 0x12800`
//!   (`lui a0,0x1` / `ori a0,a0,0x2800`). Its index path reads `0x28` sectors
//!   (`li a1,0x28`, byte total `0x14000`), which is why the trigger lookup's
//!   fallback window holds the *next* entry's leading sectors.
//!   `see ghidra/scripts/funcs/8001f7c0.txt`.
//!
//! The per-field semantics of each region (wall nibbles, floor tiers, the four
//! trigger kinds) live in
//! [`docs/subsystems/field-locomotion.md`](../../../../docs/subsystems/field-locomotion.md);
//! this module is the container: region map, trigger-block header, and the
//! detector that recognises the format from bytes alone.
//!
//! ### Detection
//!
//! `size == 0x12000` **and** a self-consistent trigger-block header: the four
//! `(offset, count)` pairs chain back-to-back at their kind strides from the
//! end of the `0x12` -byte header, each sub-table separated by a 2-byte gap,
//! and the `u16` at `+0x00` equal to the end of the last sub-table. Across the
//! whole PROT corpus that chain matches 100 entries with **zero** false
//! positives, and no entry outside the `0x12000` size class satisfies it. One
//! `0x12000` entry ships an all-zero trigger header (a scene with no walkable
//! field); it is accepted with [`FieldMap::trigger_block`] `None`.

/// Total on-disc footprint of a field map, in bytes.
pub const FIELD_MAP_BYTES: usize = 0x1_2000;

/// Offset of the object / actor descriptor table.
pub const OBJECT_RECORDS_OFFSET: usize = 0x0000;
/// Byte length of the object / actor descriptor table.
pub const OBJECT_RECORDS_BYTES: usize = 0x4000;
/// Stride of one object descriptor (`sll v0,s2,0x5` in `FUN_8003A55C`).
pub const OBJECT_RECORD_STRIDE: usize = 0x20;
/// Descriptor slots the table holds (the cell index field is 9 bits wide).
pub const OBJECT_RECORD_COUNT: usize = OBJECT_RECORDS_BYTES / OBJECT_RECORD_STRIDE;

/// Offset of the collision + floor grid.
pub const COLLISION_GRID_OFFSET: usize = 0x4000;
/// Byte length of the collision + floor grid.
pub const COLLISION_GRID_BYTES: usize = 0x4000;
/// Row stride of the collision grid (`addiu v0,v0,0x4000` after `sll ...,0x7`).
pub const COLLISION_ROW_STRIDE: usize = 0x80;
/// Tiles per axis. Both grids are square and share this dimension.
pub const GRID_DIM: usize = 128;

/// Offset of the per-tile object-index map.
pub const OBJECT_GRID_OFFSET: usize = 0x8000;
/// Byte length of the per-tile object-index map.
pub const OBJECT_GRID_BYTES: usize = 0x8000;
/// Row stride of the object-index map (`andi v0,v0,0x7f00`, u16 per tile).
pub const OBJECT_GRID_ROW_STRIDE: usize = 0x100;
/// Mask selecting a cell's object-descriptor index (`andi s2,v0,0x1ff`).
pub const OBJECT_GRID_INDEX_MASK: u16 = 0x01FF;

/// Offset of the per-tile trigger block.
pub const TRIGGER_BLOCK_OFFSET: usize = 0x1_0000;
/// Byte length of the per-tile trigger block.
pub const TRIGGER_BLOCK_BYTES: usize = 0x2000;
/// Bytes of trigger-block header before the first sub-table body.
pub const TRIGGER_HEADER_BYTES: usize = 0x12;
/// Bytes of padding the header's end offsets leave after each sub-table.
pub const TRIGGER_SUBTABLE_GAP: usize = 2;
/// Record stride per trigger kind (`DAT_8007B318 + kind`).
pub const TRIGGER_KIND_STRIDES: [usize; 4] = [4, 4, 4, 8];

/// One trigger sub-table, as named by the block header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerSubTable {
    /// Trigger kind (`0` teleports, `1` P2-record triggers, `2` elevation
    /// overrides, `3` region AABBs).
    pub kind: u8,
    /// Body offset, relative to the block base.
    pub offset: usize,
    /// Record count.
    pub count: usize,
    /// Record stride in bytes.
    pub stride: usize,
}

impl TriggerSubTable {
    /// Byte range of the sub-table body, relative to the block base.
    pub fn body_range(&self) -> std::ops::Range<usize> {
        self.offset..self.offset + self.count * self.stride
    }
}

/// The `+0x10000` trigger-block header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerBlock {
    /// The `u16` at `+0x00`: the end offset of the last sub-table.
    pub end_offset: usize,
    /// The four sub-tables, in kind order.
    pub tables: [TriggerSubTable; 4],
}

/// A recognised field map, borrowing the whole `0x12000`-byte entry.
#[derive(Debug, Clone, Copy)]
pub struct FieldMap<'a> {
    bytes: &'a [u8],
    trigger_block: Option<TriggerBlock>,
}

/// Parse the trigger-block header out of a `+0x10000..+0x12000` slice.
///
/// Returns `Ok(None)` for the one retail entry whose header is all zeros, and
/// `Err(())` when the header is present but does not chain.
///
/// REF: FUN_801D5AE0
fn parse_trigger_block(block: &[u8]) -> Result<Option<TriggerBlock>, ()> {
    if block.len() < TRIGGER_HEADER_BYTES {
        return Err(());
    }
    let u16_at = |off: usize| u16::from_le_bytes([block[off], block[off + 1]]) as usize;
    if block[..TRIGGER_HEADER_BYTES].iter().all(|&b| b == 0) {
        return Ok(None);
    }
    let mut tables = [TriggerSubTable {
        kind: 0,
        offset: 0,
        count: 0,
        stride: 4,
    }; 4];
    let mut cursor = TRIGGER_HEADER_BYTES;
    for kind in 0..4usize {
        // FUN_801D5AE0: `lh v0,0x2(v1)` / `lh v1,0x4(v1)` with `v1 = block + kind*4`.
        let offset = u16_at(4 * kind + 2);
        let count = u16_at(4 * kind + 4);
        let stride = TRIGGER_KIND_STRIDES[kind];
        if offset != cursor {
            return Err(());
        }
        cursor = offset + count * stride + TRIGGER_SUBTABLE_GAP;
        if cursor > block.len() {
            return Err(());
        }
        tables[kind] = TriggerSubTable {
            kind: kind as u8,
            offset,
            count,
            stride,
        };
    }
    if u16_at(0) != cursor {
        return Err(());
    }
    Ok(Some(TriggerBlock {
        end_offset: cursor,
        tables,
    }))
}

/// Recognise a per-scene field map. See the module docs for the criteria.
pub fn detect(buf: &[u8]) -> Option<FieldMap<'_>> {
    if buf.len() != FIELD_MAP_BYTES {
        return None;
    }
    let block = &buf[TRIGGER_BLOCK_OFFSET..TRIGGER_BLOCK_OFFSET + TRIGGER_BLOCK_BYTES];
    let trigger_block = parse_trigger_block(block).ok()?;
    Some(FieldMap {
        bytes: buf,
        trigger_block,
    })
}

impl<'a> FieldMap<'a> {
    /// The whole entry.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The trigger-block header, or `None` when the entry ships it zeroed.
    pub fn trigger_block(&self) -> Option<TriggerBlock> {
        self.trigger_block
    }

    /// The `+0x0000..+0x4000` object / actor descriptor table.
    pub fn object_records(&self) -> &'a [u8] {
        &self.bytes[OBJECT_RECORDS_OFFSET..OBJECT_RECORDS_OFFSET + OBJECT_RECORDS_BYTES]
    }

    /// One `0x20`-byte object descriptor.
    pub fn object_record(&self, index: usize) -> Option<&'a [u8]> {
        let base = OBJECT_RECORDS_OFFSET + index * OBJECT_RECORD_STRIDE;
        (index < OBJECT_RECORD_COUNT).then(|| &self.bytes[base..base + OBJECT_RECORD_STRIDE])
    }

    /// The `+0x4000..+0x8000` collision + floor grid.
    pub fn collision_grid(&self) -> &'a [u8] {
        &self.bytes[COLLISION_GRID_OFFSET..COLLISION_GRID_OFFSET + COLLISION_GRID_BYTES]
    }

    /// One collision byte. High nibble = sub-cell wall bits, low nibble =
    /// floor-elevation tier.
    pub fn collision_byte(&self, col: usize, row: usize) -> Option<u8> {
        (col < GRID_DIM && row < GRID_DIM)
            .then(|| self.bytes[COLLISION_GRID_OFFSET + row * COLLISION_ROW_STRIDE + col])
    }

    /// The `+0x8000..+0x10000` per-tile object-index map.
    pub fn object_grid(&self) -> &'a [u8] {
        &self.bytes[OBJECT_GRID_OFFSET..OBJECT_GRID_OFFSET + OBJECT_GRID_BYTES]
    }

    /// One object-index cell word.
    pub fn object_cell(&self, col: usize, row: usize) -> Option<u16> {
        if col >= GRID_DIM || row >= GRID_DIM {
            return None;
        }
        let off = OBJECT_GRID_OFFSET + row * OBJECT_GRID_ROW_STRIDE + col * 2;
        Some(u16::from_le_bytes([self.bytes[off], self.bytes[off + 1]]))
    }

    /// The `+0x10000..+0x12000` trigger block.
    pub fn trigger_bytes(&self) -> &'a [u8] {
        &self.bytes[TRIGGER_BLOCK_OFFSET..TRIGGER_BLOCK_OFFSET + TRIGGER_BLOCK_BYTES]
    }

    /// Records of one trigger sub-table, each `stride` bytes wide.
    pub fn trigger_records(&self, kind: u8) -> Vec<&'a [u8]> {
        let Some(block) = self.trigger_block else {
            return Vec::new();
        };
        let Some(t) = block.tables.get(usize::from(kind)) else {
            return Vec::new();
        };
        let body = self.trigger_bytes();
        (0..t.count)
            .filter_map(|i| {
                let at = t.offset + i * t.stride;
                body.get(at..at + t.stride)
            })
            .collect()
    }

    /// Non-zero share of the collision grid - a cheap "does this scene have a
    /// walkable field at all" probe. A handful of cutscene-only scenes ship an
    /// entirely zero grid.
    pub fn collision_fill(&self) -> f32 {
        let g = self.collision_grid();
        g.iter().filter(|&&b| b != 0).count() as f32 / g.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic field map with the given sub-table counts.
    fn synth(counts: [usize; 4]) -> Vec<u8> {
        let mut buf = vec![0u8; FIELD_MAP_BYTES];
        // A little content in each region so the entry isn't degenerate.
        buf[OBJECT_RECORDS_OFFSET + 0x12] = 0x10;
        buf[COLLISION_GRID_OFFSET] = 0xF5;
        buf[OBJECT_GRID_OFFSET] = 0x01;

        let blk = TRIGGER_BLOCK_OFFSET;
        let mut cursor = TRIGGER_HEADER_BYTES;
        for kind in 0..4usize {
            let off = cursor;
            let cnt = counts[kind];
            buf[blk + 4 * kind + 2..blk + 4 * kind + 4]
                .copy_from_slice(&(off as u16).to_le_bytes());
            buf[blk + 4 * kind + 4..blk + 4 * kind + 6]
                .copy_from_slice(&(cnt as u16).to_le_bytes());
            cursor = off + cnt * TRIGGER_KIND_STRIDES[kind] + TRIGGER_SUBTABLE_GAP;
        }
        buf[blk..blk + 2].copy_from_slice(&(cursor as u16).to_le_bytes());
        buf
    }

    #[test]
    fn regions_sum_to_the_footprint() {
        assert_eq!(
            OBJECT_RECORDS_BYTES + COLLISION_GRID_BYTES + OBJECT_GRID_BYTES + TRIGGER_BLOCK_BYTES,
            FIELD_MAP_BYTES
        );
        assert_eq!(COLLISION_ROW_STRIDE * GRID_DIM, COLLISION_GRID_BYTES);
        assert_eq!(OBJECT_GRID_ROW_STRIDE * GRID_DIM, OBJECT_GRID_BYTES);
        assert_eq!(OBJECT_RECORD_COUNT, 512);
    }

    #[test]
    fn detects_a_chained_trigger_header() {
        let buf = synth([11, 37, 235, 14]);
        let fm = detect(&buf).expect("synthetic field map");
        let blk = fm.trigger_block().expect("header");
        assert_eq!(blk.tables[0].offset, 0x12);
        assert_eq!(blk.tables[0].count, 11);
        assert_eq!(blk.tables[3].stride, 8);
        assert_eq!(blk.end_offset, blk.tables[3].body_range().end + 2);
        assert_eq!(fm.trigger_records(0).len(), 11);
        assert_eq!(fm.trigger_records(3).len(), 14);
        assert_eq!(fm.trigger_records(3)[0].len(), 8);
    }

    #[test]
    fn accepts_an_all_zero_trigger_header() {
        let mut buf = vec![0u8; FIELD_MAP_BYTES];
        buf[COLLISION_GRID_OFFSET] = 0x0F;
        let fm = detect(&buf).expect("zeroed-header field map");
        assert!(fm.trigger_block().is_none());
        assert!(fm.trigger_records(1).is_empty());
    }

    #[test]
    fn rejects_a_broken_chain() {
        let mut buf = synth([11, 37, 235, 14]);
        // Nudge sub-table 2's offset off the chain.
        let blk = TRIGGER_BLOCK_OFFSET;
        buf[blk + 4 * 2 + 2] = buf[blk + 4 * 2 + 2].wrapping_add(4);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_a_wrong_end_offset() {
        let mut buf = synth([1, 1, 1, 1]);
        let blk = TRIGGER_BLOCK_OFFSET;
        buf[blk] = buf[blk].wrapping_add(2);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_the_wrong_size() {
        let mut buf = synth([1, 1, 1, 1]);
        buf.truncate(FIELD_MAP_BYTES - 0x800);
        assert!(detect(&buf).is_none());
        let mut long = synth([1, 1, 1, 1]);
        long.resize(FIELD_MAP_BYTES + 0x800, 0);
        assert!(detect(&long).is_none());
    }

    #[test]
    fn region_accessors_address_the_documented_offsets() {
        let buf = synth([1, 1, 1, 1]);
        let fm = detect(&buf).unwrap();
        assert_eq!(fm.object_record(0).unwrap()[0x12], 0x10);
        assert_eq!(fm.collision_byte(0, 0), Some(0xF5));
        assert_eq!(fm.object_cell(0, 0), Some(0x0001));
        assert_eq!(fm.collision_byte(GRID_DIM, 0), None);
        assert_eq!(fm.object_cell(0, GRID_DIM), None);
    }
}
